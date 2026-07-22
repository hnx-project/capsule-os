#pragma once
#include <sys/types.h>

struct stat {
    off_t     st_size;
    mode_t    st_mode;
};

#define S_IFMT   0xF000
#define S_IFDIR  0x4000
#define S_IFREG  0x8000

int stat(const char *path, struct stat *buf);
int mkdir(const char *path);
int rmdir(const char *path);
