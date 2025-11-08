from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI()


class ChatRequest(BaseModel):
    model: str
    messages: list


@app.post("/chat/completions")
async def chat_completions(_: ChatRequest):
    return {"choices": [{"message": {"content": "hello from mock proxy"}}]}
