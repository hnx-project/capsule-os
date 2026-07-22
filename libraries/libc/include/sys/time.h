#pragma once

struct timespec {
    long long tv_sec;
    long long tv_nsec;
};

int nanosleep(const struct timespec *req, struct timespec *rem);
