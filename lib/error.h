#pragma once

#include <stdarg.h>
#include <stdio.h>

struct Error
{
	int code;
	char error_str[1024];
};

typedef struct Error Error;

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 3, 4)))
#endif
static inline void fill_error(Error* err, int code, const char* fmt, ...)
{
	if (!err)
		return;
	err->code = code;
	va_list args;
	va_start(args, fmt);
	vsnprintf(err->error_str, sizeof(err->error_str), fmt, args);
	va_end(args);
}

#define ERROR(err, code, fmt, ...)                                                                 \
	{                                                                                              \
		fill_error(err, code, fmt, ##__VA_ARGS__);                                                 \
		return;                                                                                    \
	}

#define OK_OR_ABORT(err)                                                                           \
	{                                                                                              \
		if (err->code)                                                                             \
			return;                                                                                \
	}
