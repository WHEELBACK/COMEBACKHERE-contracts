# Invoice Contract

The invoice contract manages invoice creation, payment, and merchant-scoped
invoice queries.

## Entrypoints

### `get_invoices_by_merchant(merchant)`

> **Deprecated.** This entrypoint returns every invoice for a merchant in a
> single call. For active merchants the result set grows without bound and can
> eventually exceed Soroban's read and memory budget, causing the call to fail.
> It is kept working for backwards compatibility per the deprecation policy in
> `CONTRIBUTING.md`, but new integrations should use
> `get_invoices_by_merchant_page` instead.

Returns all invoices belonging to `merchant`.

### `get_invoices_by_merchant_page(merchant, cursor, limit)`

Cursor-paginated replacement for `get_invoices_by_merchant`. Returns a bounded
page of invoices for `merchant`.

- `merchant`: the merchant whose invoices are being queried.
- `cursor`: opaque pagination cursor. Pass `None` (or the contract's empty
  cursor value) to start from the beginning; pass the cursor returned by the
  previous page to fetch the next page.
- `limit`: maximum number of invoices to return in this page. The contract caps
  this value, so callers may request a large `limit` but will never receive more
  than the configured maximum per call.

The response includes the page of invoices plus the cursor to use for the next
page. When the returned cursor is empty, there are no further pages.

## Client integration guide

When listing invoices for a merchant, use the paginated entrypoint:

1. Call `get_invoices_by_merchant_page(merchant, None, limit)` to fetch the
   first page.
2. Process the returned invoices.
3. If the returned cursor is non-empty, call
   `get_invoices_by_merchant_page(merchant, cursor, limit)` again with that
   cursor to fetch the next page.
4. Repeat until the returned cursor is empty.

This keeps each call bounded and avoids exceeding Soroban's read and memory
budget for merchants with many invoices. The unpaginated
`get_invoices_by_merchant` entrypoint remains available for existing
integrators but is deprecated and should not be used in new code.
