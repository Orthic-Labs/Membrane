from index import ContextIndex

index = ContextIndex()

def answer_context_question(question: str) -> list[str]:
    return index.retrieve(question, limit=3)
