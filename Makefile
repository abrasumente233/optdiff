hello.ll:
	clang hello.c -O2 -mllvm -print-after-all -mllvm -print-before-all -mllvm -filter-print-funcs="a" -S -emit-llvm
