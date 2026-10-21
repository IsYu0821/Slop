#include <stdio.h>
#include <unistd.h>
int main(int argc, char **argv) {
    printf("nopie-helper ran argc=%d arg=%s\n", argc, argc > 1 ? argv[1] : "-");
    return 17;
}
