GREETINGS = {"en": "Hello"}


def greet(name="world", lang="en"):
    return f"{GREETINGS[lang]}, {name or 'world'}!"
