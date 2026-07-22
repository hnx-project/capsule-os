#pragma once
#include <sys/types.h>

struct Dirent {
    ino_t     ino;
    size_t    size;
    unsigned char ftype;
    unsigned char name_len;
    char      name[110];
};

ssize_t readdir(int fd, void *buf, size_t count);
