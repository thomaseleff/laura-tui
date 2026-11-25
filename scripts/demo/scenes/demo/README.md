# hello-api

A small API that greets each visitor by name.

## Run

```bash
python app.py
curl "http://127.0.0.1:8766/hello?name=Ada"
```

The API answers `Hello, Ada!`. Add `&lang=es` for `Hola, Ada!`.

## Files

- `hello.py`: the `greet` function.
- `app.py`: the API, which logs each request.
- `page.html`: a page that greets the name you type.
- `plan.md`: the plan for the next change.
