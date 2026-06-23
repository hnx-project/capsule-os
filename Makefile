.PHONY: all build kernel kernel-release run clean

ARCH ?= aarch64
RUST_TARGET = aarch64-unknown-none
KERNEL_ARTIFACT = build/target/$(RUST_TARGET)/release/libkernel.a
KERNEL_ELF = kernel.elf
LINKER_SCRIPT = kernel/kernel.ld
LDD = ~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld

all: kernel

build:
	cargo build --target $(RUST_TARGET) -p kernel

kernel: build
	$(LDD) -flavor gnu -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) -o $(KERNEL_ELF)

kernel-release:
	cargo build --target $(RUST_TARGET) -p kernel --release
	$(LDD) -flavor gnu -T $(LINKER_SCRIPT) $(KERNEL_ARTIFACT) -o $(KERNEL_ELF)

run: kernel
	qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic -kernel $(KERNEL_ELF)

clean:
	rm -rf build/ target/ $(KERNEL_ELF)

check:
	cargo check --target $(RUST_TARGET) -p kernel
	cargo check --workspace
