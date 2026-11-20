-- Senders can pay in any supported currency: bank transfer (NGN) or mobile money (the rest).
ALTER TABLE deposits ADD COLUMN method TEXT NOT NULL DEFAULT 'bank_transfer';

-- sender_account now holds a tagged Recipient (the refund destination). Existing rows were NGN
-- bank accounts without the tag.
UPDATE deposits
SET sender_account = json_set(sender_account, '$.rail', 'NG_BANK_TRANSFER')
WHERE sender_account IS NOT NULL AND json_extract(sender_account, '$.rail') IS NULL;
