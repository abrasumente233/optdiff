__attribute__((warn_unused_result))
float a() {
  return 0.0;
}

int square(int x) {
  a();
  return x * x;
}
