#!/bin/bash

options=$1

assert() {
  expected="$1"
  input="$2"
  ./target/debug/dynamite_compiler "$input" > ./temp/main.s
  if [ $? != 0 ]; then
    exit 1
  fi
  cc $options -o ./temp/main ./temp/main.s
  if [ $? != 0 ]; then
    exit 1
  fi
  ./temp/main
  actual="$?"

  if [ "$actual" = "$expected" ]; then
    echo "$input => $actual"
  else
    echo "$input => $expected expected, but got $actual"
    exit 1
  fi
}

assert_stdout() {
  expected="$1"
  input="$2"
  ./target/debug/dynamite_compiler "$input" > ./temp/main.s
  if [ $? != 0 ]; then
    exit 1
  fi
  cc $options -o ./temp/main ./temp/main.s
  if [ $? != 0 ]; then
    exit 1
  fi
  actual=`./temp/main`

  if [ "$actual" = "$expected" ]; then
    echo "( $input ) => ( $actual )"
  else
    echo "( $input ) => ( $actual ) expected, but got ( $actual )"
    exit 1
  fi
}

if [ ! -d ./temp ]; then
  mkdir ./temp
fi
<< CMT
CMT

assert 0 './test/test.c'
assert 12 'int main() { int a=3; int b=4; return a*b; }'
assert 1 'int main() { int a=3; int b=4; return a*a*b==36; }'
assert 31 'int main() { int ice=3; int cream=7; return ice*cream+ice+cream; }'
assert 27 'int main() { int a=3; int c; int b = c = a; return a*b*c; }'
assert 66 'int main() { int a=47; int b=19; return a+b; return a%b; }'
assert 5 'int main() { if (3*6>15) return 5; else return 10; }'
assert 7 'int main() { int x = 5; if (x > 4) x = x+5; return x-3; }'
assert 12 'int main() { int a = 3; while (a*a < 100) a=a+3; a; }'
assert 45 'int main() { int a = 0; int i; for(i = 0; i < 10; i = i+1) a = a+i; return a; }'
assert 12 'int main() { int a = 0; for (;; a = a+3) if (a >= 10) break; return a; }'
assert 50 'int main() { int a = 0; int i; int j; for (i = 0; i < 10; i = i+1) { j = 0; while (j < 5) { a = a+1; j = j+1; } } return a; }'
assert 0 'int main() { int i; for (i = 0; i < 1;) { break; } return i; }'
assert 10 'int main() { int a = 0; int j = 0; while (1) { if (j>=5) break; a = a+j; j = j+1; } return a; }'
assert 50 'int main() { int a = 0; int i; for (i = 0; i < 10; i = i+1) { int j = 0; while (1) { if (j>=5) break; a = a+1; j = j+1; } } return a; }'
assert 104 'int add6(int a, int b, int c, int d, int e, int f) { return a + b*2 + c*3 + d*4 + e*5 + f*6; } int Add(int a, int b) { return a*2+b; } int main() { return add6(Add(3, 8), 2, 3, 4, 5, 6); }'
assert 233 'int fib(int i) { if (i == 0) return 0; if(i == 1) return 1; return fib(i-1) + fib(i-2); } int main() { return fib(13); } '
assert 3 'int main() { int x; int *y; y = &x; *y = 3; return x; }'
assert 4 'int main() { return sizeof(sizeof(1));}'
assert 4 'int main() { return sizeof(8);}'
assert 8 'int main() { int *y; return sizeof(y);}'
assert 4 'int main() { int *y; return sizeof *y;}'
assert 12 'int main() {int a[3]; return sizeof a;}'
assert 3 'int main() {int a[2];*a = 1;*(a + 1) = 2;int *p;p = a;return *p + *(p + 1);}'
assert 8 'int main() {int a[3]; a[0] = 8; a[3] = 9; return a[0];} '
assert 9 'int main() {int a[3]; a[0] = 8; a[3] = 9; return 3[a];} '
assert 4 'int b; int main() { b = 4; return b;} '
assert 9 'int arr[100];  int c; int main() { c = 4; arr[10] = 5; arr[7] = 7; return c+arr[10];}'
assert 3 'int main() { char x[3]; x[0] = -1; x[1] = 2; int y; y = 4; return x[0] + y; }'
assert 3 'char x[3]; int main() {  x[0] = -1; x[1] = 2; int y; y = 4; return x[0] + y; }'
assert_stdout "value = 777" 'int value; int main() { value = 777; printf("value = %d\n", value); return 0;}'
assert_stdout "value = 755" 'int value; int main() { char* fmt = "value = %d\n"; value = 755; printf(fmt, value); return 0;}'
assert_stdout "value = 222" 'char* fmt; int main() { fmt = "value = %d\n"; printf(fmt, 222); return 0;}'

echo OK