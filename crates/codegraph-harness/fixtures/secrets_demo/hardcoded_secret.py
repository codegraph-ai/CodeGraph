# Test fixture for secrets_entropy detector. Contains a fake-but-realistic
# AWS-style key so the entropy heuristic should flag it.

AWS_ACCESS_KEY_ID = "AKIAIOSFODNN7EXAMPLE"
AWS_SECRET_ACCESS_KEY = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"


def connect():
    return {"key": AWS_ACCESS_KEY_ID, "secret": AWS_SECRET_ACCESS_KEY}
