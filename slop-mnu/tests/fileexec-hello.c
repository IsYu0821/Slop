/* Clean-room target for the file-backed execve probe. */
#include <stdio.h>
#include <unistd.h>

int main(void) {
    printf("fileexec-hello: hello from disk pid=%d\n", (int)getpid());
    fflush(stdout);
    return 0;
}
