from app import answer_context_question

def test_context_question_uses_index() -> None:
    assert answer_context_question("repository architecture") == ["architecture contract", "repository graph"]
