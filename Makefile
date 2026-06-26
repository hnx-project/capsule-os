.PHONY: all build clean check help bootloader run-ohc ohc

ARCH ?= aarch64

ifeq ($(ARCH),aarch64)
    BOOT_TARGET = aarch64-unknown-none
    BOOTLOADER_DIR = build/target/aarch64-unknown-none/release
    QEMU_ARCH = aarch64
    QEMU_MACHINE = virt
    QEMU_CPU = cortex-a72
    QEMU_MEM = 512M
    QEMU_EXTRA = 
    DTB_ADDR = 0x42000000
    OHC_ADDR = 0x40700000
else ifeq ($(ARCH),riscv64)
    BOOT_TARGET = riscv64gc-unknown-none-elf
    BOOTLOADER_DIR = build/target/riscv64gc-unknown-none-elf/release
    QEMU_ARCH = riscv64
    QEMU_MACHINE = virt
    QEMU_CPU = rv64
    QEMU_MEM = 512M
    QEMU_EXTRA = -bios default
    DTB_ADDR = 0x82000000
    OHC_ADDR = 0x80700000
endif

OHC_FILE = dist/kernel/hnxcore.ohc
OBJCOPY = /opt/homebrew/opt/llvm/bin/llvm-objcopy
BOOTLOADER_ELF = $(BOOTLOADER_DIR)/capsule-bootloader
BOOTLOADER_BIN = $(BOOTLOADER_DIR)/capsule-bootloader.bin

all: run-ohc

help:
	@echo "CapsuleOS Build System"
	@echo ""
	@echo "Targets:"
	@echo "  bootloader     - Build capsule-bootloader"
	@echo "  ohc            - Delegate to hnx-core to compile and copy hnxcore.ohc"
	@echo "  run-ohc        - Run CapsuleOS CLI system in QEMU"
	@echo "  clean          - Clean build artifacts"
	@echo ""

dist:
	mkdir -p dist/kernel

build:
	cargo build --workspace

clean:
	rm -rf dist/ build/ target/ hnxcore.ohc
	$(MAKE) -C kernel clean

check:
	cargo check --workspace
	$(MAKE) -C kernel check

ohc: dist
	@echo "Delegating kernel build to hnx-core..."
	$(MAKE) -C kernel ohc ARCH=$(ARCH)
	cp kernel/dist/kernel/hnxcore.ohc dist/kernel/hnxcore.ohc

bootloader:
	rustup run stable cargo build --release -p capsule-bootloader --target $(BOOT_TARGET)
	$(OBJCOPY) -O binary $(BOOTLOADER_ELF) $(BOOTLOADER_BIN)

DTB_FILE = dist/qemu.dtb

run-ohc: ohc bootloader $(DTB_FILE)
	@echo "Running CapsuleOS with bootloader ($(ARCH))..."
	@echo "Use Ctrl+A, X to exit"
	qemu-system-$(QEMU_ARCH) \
		-M $(QEMU_MACHINE) -cpu $(QEMU_CPU) -m $(QEMU_MEM) -nographic \
		-kernel $(BOOTLOADER_ELF) \
		-device loader,file=$(OHC_FILE),addr=$(OHC_ADDR),force-raw=on \
		-device loader,file=$(DTB_FILE),addr=$(DTB_ADDR),force-raw=on \
		$(QEMU_EXTRA) \
		-semihosting

$(DTB_FILE):
	@mkdir -p dist
	@echo "Generating QEMU DTB for $(ARCH)..."
	@qemu-system-$(QEMU_ARCH) -M $(QEMU_MACHINE) -cpu $(QEMU_CPU) -m $(QEMU_MEM) -machine dumpdtb=/tmp/qemu_raw.dtb $(QEMU_EXTRA) -display none > /dev/null 2>&1
	@dtc -I dtb -O dts /tmp/qemu_raw.dtb -o /tmp/qemu.dts
	@dtc -I dts -O dtb /tmp/qemu.dts -o $(DTB_FILE)
	@rm -f /tmp/qemu_raw.dtb /tmp/qemu.dts
