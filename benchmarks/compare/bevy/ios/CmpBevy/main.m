// The whole app is the Rust static library (../src/lib.rs); winit creates
// the UIApplication inside main_rs().
extern void main_rs(void);

int main(int argc, char *argv[]) {
    (void)argc;
    (void)argv;
    main_rs();
    return 0;
}
