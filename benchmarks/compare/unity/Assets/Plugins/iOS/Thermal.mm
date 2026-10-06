// Engine comparison (docs/engine-comparison.md): the thermal state and a
// stdout writer, so the `[cmp]` line reaches `xcrun devicectl ... --console`
// (which streams the process's stdout/stderr) and the Xcode console.
#import <Foundation/Foundation.h>
#include <stdio.h>

extern "C" int cmp_thermal_state(void)
{
    // 0 nominal, 1 fair, 2 serious, 3 critical.
    return (int)[[NSProcessInfo processInfo] thermalState];
}

extern "C" void cmp_print(const char* line, int use_nslog)
{
    if (line == NULL) return;
    if (use_nslog) {
        NSLog(@"%s", line);
    } else {
        fputs(line, stdout);
        fputc('\n', stdout);
        fflush(stdout);
    }
}
