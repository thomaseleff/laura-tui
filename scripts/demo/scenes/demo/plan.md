# Auth tokens — draft spec

Review before we ship.

- Tokens are issued on login and returned in the JSON body.
- Every token expires 24 hours after it is issued.
- A refresh token extends a session up to 30 days.
- Tokens are stored in local storage on the client.
- Revoked tokens are checked against a denylist on each request.

Open question: should refresh tokens rotate on every use?
