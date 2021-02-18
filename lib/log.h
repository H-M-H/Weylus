#pragma once

#include <stdarg.h>
#include <stdio.h>

void log_debug(const char* fmt, ...);
void log_info(const char* fmt, ...);
void log_trace(const char* fmt, ...);
void log_warn(const char* fmt, ...);
