# TypeScript Integration Examples

These examples show the expected call flow from an off-chain service. They use
plain `fetch` wrappers so teams can adapt them to their Stellar SDK transport of
choice.

```ts
type ContractInvoke<T> = {
  contractId: string;
  method: string;
  args: Record<string, unknown>;
  result: T;
};

async function invoke<T>(request: Omit<ContractInvoke<T>, "result">): Promise<T> {
  const response = await fetch("/api/stellar/invoke", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(await response.text());
  return (await response.json()) as T;
}
```

## Create and settle an invoice

```ts
export async function createAndSettleInvoice(input: {
  invoiceId: string;
  invoiceContract: string;
  workflowContract: string;
  merchant: string;
  signer: string;
  tokenContract: string;
  amount: string;
}) {
  await invoke<number>({
    contractId: input.invoiceContract,
    method: "mark_paid",
    args: { admin: input.signer, id: input.invoiceId, payer: input.merchant },
  });

  return invoke<void>({
    contractId: input.workflowContract,
    method: "execute_with_compliance",
    args: {
      settlement_id: input.invoiceId,
      token_contract: input.tokenContract,
      merchant: input.merchant,
    },
  });
}
```

## Read protocol health as JSON

```ts
export async function loadProtocolHealth() {
  const response = await fetch("/ops/protocol-health.json");
  if (!response.ok) throw new Error("protocol health unavailable");
  return response.json() as Promise<{
    network: string;
    contracts: {
      treasury: { pending_settlements: number; pause_state: string };
      compliance: { blocked_addresses: number; pause_state: string };
      invoice: { invoice_count: number; pause_state: string };
    };
  }>;
}
```
