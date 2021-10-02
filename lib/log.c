#include "log.h"

// rust functions living in log.rs
void log_error_rust(const char*);
void log_debug_rust(const char*);
void log_info_rust(const char*);
void log_trace_rust(const char*);
void log_warn_rust(const char*);

void log_error(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	log_error_rust(buf);
}

void log_debug(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	log_debug_rust(buf);
}

void log_info(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	log_info_rust(buf);
}

void log_trace(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	log_trace_rust(buf);
}

void log_warn(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	log_warn_rust(buf);
}
