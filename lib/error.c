#include "error.h"

void fill_error(Error* err, int code, const char* fmt, ...)
{
	if (!err)
		return;
	err->code = code;
	va_list args;
	va_start(args, fmt);
	vsnprintf(err->error_str, sizeof(err->error_str), fmt, args);
}
