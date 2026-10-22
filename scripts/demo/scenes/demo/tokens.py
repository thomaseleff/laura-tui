def issue(user):
    ttl = 24 * 3600
    token = sign(user, ttl)
    store_local(token)
    return token
