from typing import Any


class UnknownDetails:
    def __repr__(self) -> str:
        return "UnknownDetails()"


UNKNOWN_DETAILS = UnknownDetails()


class MomentoError(Exception):
    def __init__(self, message: str, details: Any) -> None:
        super().__init__(message)
        self.message = message
        self.details = details


class AuthenticationError(MomentoError):
    pass


class AuthorizationError(MomentoError):
    pass


class NotFoundError(MomentoError):
    pass


class ValidationError(MomentoError):
    pass


class MediaProcessingError(MomentoError):
    pass


class ImportError(MomentoError):
    pass


class ConfigurationError(MomentoError):
    pass
