.PHONY: all build kernel kernel-release stage1 capsule-os.bin clean check help run

ARCH ?= aarch64
RUST_TARGET = aarch64-unknown-none
KERNEL_ARTIFACT = dist/kernel/libkernel.a
KERNEL_ELF = dist/kernel/kernel.elf
KERNEL_BIN = dist/kernel/kernel.bin
OHC_FILE = dist/kernel/hnxcore.ohc
STAGE1_ASM = kernel/boot/stage1.S
STAGE1_BIN = dist/stage1.bin
STAGE1_ELF = dist/stage1.elf
CAPSULE_OS = dist/capsule-os.bin
LINKER_SCRIPT = kernel/kernel.ld
STAGE1_LD = kernel/boot/stage1.ld
DTB = config/dtb/qemu.dtb
LDD = ~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld
OBJCOPY = /opt/homebrew/opt/llvm/bin/llvm-objcopy
AS = clang
ASFLAGS = -target aarch64-elf
OHC_TOOL = cargo run -p ohc-tool --

STAGE1_LOAD_ADDR = 0x40080000
KERNEL_LOAD_ADDR = 0x40081000

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

dist:
	mkdir -p dist/kernel

build: dist
	cargo build --target $(RUST_TARGET) -p kernel
	cp build/target/$(RUST_TARGET)/debug/libkernel.a $(KERNEL_ARTIFACT)

kernel: build
	$(LDD) -flavor gnu --whole-archive -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) --no-whole-archive -o $(KERNEL_ELF)

kernel-release: dist
	cargo build --target $(RUST_TARGET) -p kernel --release
	cp build/target/$(RUST_TARGET)/release/libkernel.a $(KERNEL_ARTIFACT)
	$(LDD) -flavor gnu --whole-archive -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) --no-whole-archive -o $(KERNEL_ELF)

stage1: $(STAGE1_ASM) $(STAGE1_LD) | dist
	$(AS) $(ASFLAGS) -c $(STAGE1_ASM) -o dist/stage1.o
	$(LDD) -flavor gnu -T $(STAGE1_LD) dist/stage1.o -o $(STAGE1_ELF)
	$(OBJCOPY) -O binary $(STAGE1_ELF) $(STAGE1_BIN)

$(KERNEL_BIN): kernel
	$(OBJCOPY) -O binary $(KERNEL_ELF) $(KERNEL_BIN)

capsule-os.bin: stage1 $(KERNEL_BIN) | dist
	truncate -s $$((KERNEL_LOAD_ADDR + $(shell stat -f%z $(KERNEL_BIN)))) $(CAPSULE_OS)
	dd if=$(STAGE1_BIN) of=$(CAPSULE_OS) conv=notrunc bs=1 seek=$(STAGE1_LOAD_ADDR)
	dd if=$(KERNEL_BIN) of=$(CAPSULE_OS) conv=notrunc bs=1 seek=$(KERNEL_LOAD_ADDR)

run: capsule-os.bin
	@echo "Running CapsuleOS on QEMU..."
	@echo "Use Ctrl+A, X to exit"
	qemu-system-aarch64 -M virt -cpu cortex-a72 -nographic \
		-kernel $(CAPSULE_OS) \
		-dtb $(DTB) \
		-semihosting

clean:
	rm -rf dist/ build/ target/ hnxcore.ohc

check:
	cargo check --target $(RUST_TARGET) -p kernel
	cargo check --workspace

ohc: kernel
	$(OHC_TOOL) pack --input $(KERNEL_ELF) --output $(OHC_FILE) --entry 1076075520
