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

## Raise a dispute

```ts
export async function raiseDispute(input: {
  disputeContract: string;
  settlementId: string;
  initiator: string;
  reason: string;
}) {
  return invoke<number>({
    contractId: input.disputeContract,
    method: "raise_dispute",
    args: {
      settlement_id: input.settlementId,
      initiator: input.initiator,
      reason: input.reason,
    },
  });
}
```

## Vote on a dispute

```ts
export async function voteOnDispute(input: {
  disputeContract: string;
  disputeId: string;
  voter: string;
  inFavor: boolean;
}) {
  return invoke<void>({
    contractId: input.disputeContract,
    method: "cast_vote",
    args: {
      dispute_id: input.disputeId,
      voter: input.voter,
      in_favor: input.inFavor,
    },
  });
}
```

## Resolve a dispute

```ts
export async function resolveDispute(input: {
  disputeContract: string;
  disputeId: string;
  resolver: string;
  outcome: "refund" | "release";
}) {
  return invoke<void>({
    contractId: input.disputeContract,
    method: "resolve_dispute",
    args: {
      dispute_id: input.disputeId,
      resolver: input.resolver,
      outcome: input.outcome,
    },
  });
}
```

## Hold and release a settlement

```ts
export async function holdSettlement(input: {
  treasuryContract: string;
  settlementId: string;
  admin: string;
  reason: string;
}) {
  return invoke<void>({
    contractId: input.treasuryContract,
    method: "hold_settlement",
    args: {
      settlement_id: input.settlementId,
      admin: input.admin,
      reason: input.reason,
    },
  });
}

export async function releaseSettlement(input: {
  treasuryContract: string;
  settlementId: string;
  admin: string;
}) {
  return invoke<void>({
    contractId: input.treasuryContract,
    method: "release_settlement",
    args: { settlement_id: input.settlementId, admin: input.admin },
  });
}
```
