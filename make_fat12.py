import os

def create_fat12_image(path):
    # Total sectors = 2880 (1.44 MB)
    sector_size = 512
    total_sectors = 2880
    image_size = total_sectors * sector_size

    # Create empty image
    image = bytearray(image_size)

    # Sector 0: Boot Sector
    # EB 3C 90
    image[0:3] = b'\xEB\x3C\x90'
    # OEM Name
    image[3:11] = b'MSDOS5.0'
    # Bytes per sector (512)
    image[11:13] = (512).to_bytes(2, 'little')
    # Sectors per cluster (1)
    image[13] = 1
    # Reserved sectors (1)
    image[14:16] = (1).to_bytes(2, 'little')
    # Number of FATs (2)
    image[16] = 2
    # Max root directory entries (224)
    image[17:19] = (224).to_bytes(2, 'little')
    # Total sectors (2880)
    image[19:21] = (2880).to_bytes(2, 'little')
    # Media descriptor (0xF8)
    image[21] = 0xF8
    # Sectors per FAT (9)
    image[22:24] = (9).to_bytes(2, 'little')
    # Sectors per track (18)
    image[24:26] = (18).to_bytes(2, 'little')
    # Number of heads (2)
    image[26:28] = (2).to_bytes(2, 'little')
    # Hidden sectors (0)
    image[28:32] = (0).to_bytes(4, 'little')
    # Huge sectors (0)
    image[32:36] = (0).to_bytes(4, 'little')
    # Drive number (0)
    image[36] = 0
    # Reserved (0)
    image[37] = 0
    # Boot signature (0x29)
    image[38] = 0x29
    # Volume ID
    image[39:43] = b'\x12\x34\x56\x78'
    # Volume Label
    image[43:54] = b'BOOTFS     '
    # File system type
    image[54:62] = b'FAT12   '

    # Signature (0x55AA)
    image[510:512] = b'\x55\xAA'

    # FAT 1: Sector 1 (offset 512)
    # FAT12 requires first two entries to be F8 FF FF
    image[512:515] = b'\xF8\xFF\xFF'

    # FAT 2: Sector 10 (offset 5120)
    image[5120:5123] = b'\xF8\xFF\xFF'

    # Ensure output directory exists
    dir_name = os.path.dirname(path)
    if dir_name:
        os.makedirs(dir_name, exist_ok=True)

    # Write out image
    with open(path, 'wb') as f:
        f.write(image)

    print(f"Successfully generated FAT12 formatted disk image at: {path}")

if __name__ == '__main__':
    create_fat12_image('disk.img')
