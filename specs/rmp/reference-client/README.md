# RMP Reference Client

The reference client is intentionally tiny and dependency-free. It demonstrates
the canonical HTTP envelope shape and operation helpers. It is not a replacement
for a production SDK.

```python
from rmp_client import RmpClient

client = RmpClient(
    base_url="http://localhost:8080",
    token="dev-token",
    principal_id="user_123",
    tenant_id="tenant_lab",
    agent_id="agent_research",
)

response = client.recall("prepare context for the Oracle employment question")
print(response["response"]["context_pack"])
```
