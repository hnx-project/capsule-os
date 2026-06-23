.PHONY: all build kernel kernel-release ohc run clean check

ARCH ?= aarch64
RUST_TARGET = aarch64-unknown-none
KERNEL_ARTIFACT = build/target/$(RUST_TARGET)/release/libkernel.a
KERNEL_ELF = kernel.elf
OHC_FILE = hnxcore.ohc
LINKER_SCRIPT = kernel/kernel.ld
LDD = ~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld
OHC_TOOL = cargo run -p ohc-tool --

all: ohc

build:
	cargo build --target $(RUST_TARGET) -p kernel

kernel: build
	$(LDD) -flavor gnu -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) -o $(KERNEL_ELF)

ohc: kernel
	$(OHC_TOOL) pack --input $(KERNEL_ELF) --output $(OHC_FILE) --entry 1076075520

kernel-release:
	cargo build --target $(RUST_TARGET) -p kernel --release
	$(LDD) -flavor gnu -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) -o $(KERNEL_ELF)
	$(OHC_TOOL) pack --input $(KERNEL_ELF) --output $(OHC_FILE) --entry 1076075520

run: ohc
	qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic -kernel $(OHC_FILE)

run-elf: kernel
	qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic -kernel $(KERNEL_ELF)

clean:
	rm -rf build/ target/ $(KERNEL_ELF) $(OHC_FILE)

check:
	cargo check --target $(RUST_TARGET) -p kernel
	cargo check --workspace
