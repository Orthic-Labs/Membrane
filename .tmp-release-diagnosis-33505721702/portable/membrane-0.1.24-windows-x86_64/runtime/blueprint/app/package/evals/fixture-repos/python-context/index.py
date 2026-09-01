class ContextIndex:
    def __init__(self) -> None:
        self._documents = ["architecture contract", "durable memory", "repository graph"]

    def retrieve(self, query: str, limit: int) -> list[str]:
        terms = set(query.lower().split())
        matches = [doc for doc in self._documents if terms.intersection(doc.split())]
        return matches[:limit]
