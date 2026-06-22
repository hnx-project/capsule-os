ARCH ?= aarch64
BOARD ?= qemu
MODE ?= release

CARGO = cargo
RUST_TARGET = $(shell cat config/machine/$(ARCH)/default.conf 2>/dev/null | grep rust_target | cut -d= -f2)

.PHONY: all build run clean test

all: build

build:
	@echo "Building Capsule OS for $(ARCH)..."
	$(CARGO) build --workspace --target $(RUST_TARGET) --release
	@echo "Build complete."

run:
	@echo "Running in QEMU..."
	qemu-system-aarch64 \
		-machine virt \
		-cpu cortex-a57 \
		-nographic \
		-kernel build/target/$(RUST_TARGET)/release/libhnx_kernel.a \
		-device virtio-serial-device \
		-serial mon:stdio

clean:
	rm -rf build/
	$(CARGO) clean --workspace

test:
	$(CARGO) test --workspace

check:
	$(CARGO) check --workspace
	$(CARGO) clippy --workspace -D warnings 2>/dev/null || true
