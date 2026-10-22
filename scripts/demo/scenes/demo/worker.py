def process(job):
    tries = 0
    while not job.done:
        tries += 1           # bug: backoff never resets between jobs
        run(job)
    return tries
