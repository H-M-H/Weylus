#pragma once

#include <stdarg.h>
#include <stdio.h>

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 1, 2)))
void log_error(const char* fmt, ...);
__attribute__((__format__ (__printf__, 1, 2)))
void log_debug(const char* fmt, ...);
__attribute__((__format__ (__printf__, 1, 2)))
void log_info(const char* fmt, ...);
__attribute__((__format__ (__printf__, 1, 2)))
void log_trace(const char* fmt, ...);
__attribute__((__format__ (__printf__, 1, 2)))
void log_warn(const char* fmt, ...);
#else
void log_error(const char* fmt, ...);
void log_debug(const char* fmt, ...);
void log_info(const char* fmt, ...);
void log_trace(const char* fmt, ...);
void log_warn(const char* fmt, ...);
#endif
