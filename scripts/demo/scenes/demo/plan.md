# Plan: greet each visitor by name

Today the greeting API says `Hello, world!` to every visitor. This plan greets each visitor by name, then serves and logs the greeting, so we can debug it.

## Steps

1. `greet Ada` prints `Hello, Ada!`, and `greet` still prints `Hello, world!`.
2. The greeting is in English only.
3. The API serves the greeting at `/hello?name=Ada`.
4. The API logs each request, with the name it greeted.
