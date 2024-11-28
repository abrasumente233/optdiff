__attribute__((warn_unused_result))
float a(int *ptr) {
  int t1 = ptr[0];
  int t2 = ptr[0] + t1;
  return t2 * ptr[0];
}

int square(int x) {
  a(&x);
  return x * x;
}
