#!/usr/bin/env python3
"""
Reference implementation of invoice amount/precision validation rules.
Used for differential testing against the Rust implementation.
"""

import sys
import json

USDC_FACTOR = 10_000_000


class ValidationError(Exception):
    """Raised when validation fails"""
    pass


def validate_amount(amount_usdc: int, gross_usdc: int) -> None:
    """
    Implements require_positive_amount and require_usdc_precision.
    Raises ValidationError if validation fails.
    """
    # require_positive_amount: both must be positive, and gross >= amount
    if amount_usdc <= 0 or gross_usdc < amount_usdc:
        raise ValidationError("InvalidAmount")

    # require_usdc_precision: both must be >= USDC_FACTOR (1 USDC in stroops)
    if amount_usdc < USDC_FACTOR or gross_usdc < USDC_FACTOR:
        raise ValidationError("AmountPrecision")


def main():
    """
    Reads JSON from stdin with {amount_usdc, gross_usdc} pairs.
    Outputs JSON {valid: true/false, error: null/"ErrorName"}
    """
    try:
        data = json.load(sys.stdin)
        amount = data.get("amount_usdc")
        gross = data.get("gross_usdc")

        if amount is None or gross is None:
            print(json.dumps({"valid": False, "error": "MissingField"}))
            sys.exit(0)

        try:
            validate_amount(amount, gross)
            print(json.dumps({"valid": True, "error": None}))
        except ValidationError as e:
            print(json.dumps({"valid": False, "error": str(e)}))

    except json.JSONDecodeError:
        print(json.dumps({"valid": False, "error": "InvalidJSON"}))
        sys.exit(1)


if __name__ == "__main__":
    main()
