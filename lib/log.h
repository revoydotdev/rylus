#pragma once

#include <stdarg.h>
#include <stdio.h>

// Rust functions living in log.rs (linked at final link time)
extern void log_error_rust(const char*);
extern void log_debug_rust(const char*);
extern void log_info_rust(const char*);
extern void log_trace_rust(const char*);
extern void log_warn_rust(const char*);

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 1, 2)))
#endif
static inline void log_error(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	va_end(args);
	log_error_rust(buf);
}

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 1, 2)))
#endif
static inline void log_debug(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	va_end(args);
	log_debug_rust(buf);
}

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 1, 2)))
#endif
static inline void log_info(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	va_end(args);
	log_info_rust(buf);
}

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 1, 2)))
#endif
static inline void log_trace(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	va_end(args);
	log_trace_rust(buf);
}

#if defined(__clang__) || defined(__GNUC__)
__attribute__((__format__ (__printf__, 1, 2)))
#endif
static inline void log_warn(const char* fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	va_end(args);
	log_warn_rust(buf);
}
