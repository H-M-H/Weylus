#include "error.h"

void fill_error(Error* err, int code, const char* fmt, ...)
{
	if (!err)
		return;
	va_list args;
	err->code = code;
	vsnprintf(err->error_str, sizeof(err->error_str), fmt, args);
}
