#pragma once
#include <sys/types.h>

int     close(int fd);
ssize_t read(int fd, void *buf, size_t count);
ssize_t write(int fd, const void *buf, size_t count);
int     unlink(const char *path);
int     rename(const char *oldpath, const char *newpath);

pid_t   getpid(void);
int     exec(const char *name);
int     execv(const char *path, char *const argv[]);

int     chdir(const char *path);
char   *getcwd(char *buf, size_t size);

int     pipe(int fds[2]);
int     dup(int oldfd);
int     dup2(int oldfd, int newfd);

int     pause(void);
int     isatty(int fd);
int     usleep(unsigned int useconds);
int     kill(pid_t pid, int sig);
