from pathlib import Path
from langchain_fireworks import FireworksEmbeddings


# 1. Charger le texte depuis un fichier
text_path = Path("../examples/example.txt")
text = text_path.read_text(encoding="utf-8")

# 2. Initialiser le modèle d'embedding Fireworks
embeddings = FireworksEmbeddings(
    model="nomic-ai/nomic-embed-text-v1.5"
)

# 3. Encoder le texte
vector = embeddings.embed_query(text)

# 4. Afficher le résultat
print(f"Embedding dimension: {len(vector)}")
print("First 10 values:")
print(vector[:10])
