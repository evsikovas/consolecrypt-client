class RunnerError(Exception):
    """A stable public reason with no raw process output or credential contents."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def require(condition: bool, code: str, message: str) -> None:
    if not condition:
        raise RunnerError(code, message)
