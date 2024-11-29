# optdiff

`optdiff` is a CLI tool that displays differences in LLVM IR between optimization passes. It's a standalone port of the Compiler Explorer [optpipeline feature](https://github.com/compiler-explorer/compiler-explorer/blob/main/static/panes/opt-pipeline.ts).

## Usage

Consider this example `square.c` file:
```c
int square(int x) {
    return x * x;
}
```

You can run `optdiff` directly with the compiler output:
```sh
clang square.c -O2 -mllvm -print-before-all -mllvm -print-after-all -c -o /dev/null 2>&1 | optdiff | delta
```

Alternatively, you can save the pass dump to a file and process it later:
```sh
clang square.c -O2 -mllvm -print-before-all -mllvm -print-after-all -c -o /dev/null &> dump.txt
optdiff dump.txt | delta
```

By default, `optdiff` outputs uncolored unified diff format. For better readability, you can pipe the output through diff visualization tools like [delta](https://github.com/dandavison/delta) or [riff](https://github.com/walles/riff).

To skip passes that don't modify the IR, use the `--skip-unchanged` or `-s` option:
```sh
optdiff dump.txt -s
```

For a complete list of available options:
```sh
optdiff --help
```
