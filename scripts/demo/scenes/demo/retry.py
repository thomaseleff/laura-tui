def backoff(tries):
    delay = 2 ** tries
    return delay
