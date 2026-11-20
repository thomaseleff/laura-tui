# Greeting — draft spec

Review before we ship.

- `greet` prints `Hello, world!`.
- The greeting is in English only.
- `greet Ada` prints `Hello, Ada!`.
- The API serves the greeting at `/hello?name=Ada`.
- The API logs each request.

Open question: should an empty name fall back to `world`?
