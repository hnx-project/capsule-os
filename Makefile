.PHONY: all build kernel kernel-release stage1 stage1.bin capsule-os.bin clean check help

ARCH ?= aarch64
RUST_TARGET = aarch64-unknown-none
KERNEL_ARTIFACT = build/target/$(RUST_TARGET)/release/libkernel.a
KERNEL_ELF = kernel.elf
KERNEL_BIN = kernel.bin
OHC_FILE = hnxcore.ohc
STAGE1_ASM = kernel/boot/stage1.S
STAGE1_BIN = stage1.bin
STAGE1_ELF = stage1.elf
CAPSULE_OS = capsule-os.bin
LINKER_SCRIPT = kernel/kernel.ld
STAGE1_LD = kernel/boot/stage1.ld
LDD = ~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld
OBJCOPY = /opt/homebrew/opt/llvm/bin/llvm-objcopy
AS = clang
ASFLAGS = -target aarch64-elf
OHC_TOOL = cargo run -p ohc-tool --

all: capsule-os.bin

help:
	@echo "CapsuleOS Build System"
	@echo ""
	@echo "Targets:"
	@echo "  build          - Build kernel library"
	@echo "  kernel         - Build and link kernel ELF"
	@echo "  kernel-release - Build kernel with release profile"
	@echo "  stage1         - Build stage1 bootloader"
	@echo "  capsule-os.bin - Build combined stage1 + kernel image"
	@echo "  run            - Run capsule-os.bin in QEMU"
	@echo "  clean          - Clean build artifacts"
	@echo ""

build:
	cargo build --target $(RUST_TARGET) -p kernel

kernel: build
	$(LDD) -flavor gnu --whole-archive -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) --no-whole-archive -o $(KERNEL_ELF)

kernel-release:
	cargo build --target $(RUST_TARGET) -p kernel --release
	$(LDD) -flavor gnu --whole-archive -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) --no-whole-archive -o $(KERNEL_ELF)

stage1: $(STAGE1_ASM) $(STAGE1_LD)
	$(AS) $(ASFLAGS) -c $(STAGE1_ASM) -o stage1.o
	$(LDD) -flavor gnu -T $(STAGE1_LD) stage1.o -o $(STAGE1_ELF)
	$(OBJCOPY) -O binary $(STAGE1_ELF) $(STAGE1_BIN)

$(KERNEL_BIN): kernel
	$(OBJCOPY) -O binary $(KERNEL_ELF) $(KERNEL_BIN)

capsule-os.bin: stage1 $(KERNEL_BIN)
	dd if=/dev/zero of=$(CAPSULE_OS) bs=1 seek=$$((0x40080000 - 0x40000000)) count=0 2>/dev/null
	dd if=$(STAGE1_BIN) of=$(CAPSULE_OS) conv=notrunc
	dd if=$(KERNEL_BIN) of=$(CAPSULE_OS) bs=1 seek=$$((0x40080000 - 0x40000000)) conv=notrunc

run: capsule-os.bin
	qemu-system-aarch64 -M virt -cpu cortex-a57 -kernel $(CAPSULE_OS) -nographic

clean:
	rm -rf build/ target/ $(KERNEL_ELF) $(KERNEL_BIN) $(STAGE1_BIN) $(STAGE1_ELF) stage1.o $(CAPSULE_OS) $(OHC_FILE)

check:
	cargo check --target $(RUST_TARGET) -p kernel
	cargo check --workspace

ohc: kernel
	$(OHC_TOOL) pack --input $(KERNEL_ELF) --output $(OHC_FILE) --entry 1076075520
