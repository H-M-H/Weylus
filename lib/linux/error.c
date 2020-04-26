#include "error.h"

void fill_error(Error* err, int code, const char* fmt, ...)
{
	va_list args;
	err->code = code;
	vsnprintf(err->error_str, sizeof(err->error_str), fmt, args);
}
