from langchain_fireworks import ChatFireworks
from langchain_core.messages import HumanMessage

# Initialize Fireworks QWen3 model
llm = ChatFireworks(
    model="accounts/fireworks/models/qwen3-vl-235b-a22b-instruct",
    temperature=0.7,
)

# User prompt
messages = [
    HumanMessage(content="Explain what Flow Matching is, briefly.")
]

# Call the model
response = llm.invoke(messages)

# Show the response
print(response.content)
