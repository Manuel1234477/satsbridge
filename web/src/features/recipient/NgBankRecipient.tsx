'use client';

import { useMemo, useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { CheckCircle2 } from 'lucide-react';
import { getPayoutInstitutions, resolveRecipient } from '@/lib/api';
import type { PayoutInstitution } from '@/lib/api/schemas';
import type { RecipientDraft } from './types';
import { maskTail } from './types';

interface NgBankRecipientProps {
  onConfirm: (draft: RecipientDraft) => void;
}

interface Resolution {
  resolutionId: string;
  accountName: string;
  bankCode: string;
  bankName: string;
  accountNumber: string;
}

export function NgBankRecipient({ onConfirm }: NgBankRecipientProps) {
  const [bankCode, setBankCode] = useState('');
  const [bankFilter, setBankFilter] = useState('');
  const [accountNumber, setAccountNumber] = useState('');
  const [resolution, setResolution] = useState<Resolution | null>(null);

  const banks = useQuery({
    queryKey: ['institutions', 'NGN', 'NG_BANK_TRANSFER'],
    queryFn: () => getPayoutInstitutions('NGN', 'NG_BANK_TRANSFER'),
    staleTime: 60 * 60 * 1000,
  });

  const verify = useMutation({
    mutationFn: (input: { bank_code: string; account_number: string }) =>
      resolveRecipient({ currency: 'NGN', ...input }),
  });

  const visibleBanks = useMemo<PayoutInstitution[]>(() => {
    const query = bankFilter.trim().toLowerCase();
    const list = banks.data ?? [];
    if (!query) return list;
    return list.filter((b) => b.name.toLowerCase().includes(query) || b.code.includes(query));
  }, [banks.data, bankFilter]);

  const selectedBank = banks.data?.find((b) => b.code === bankCode);
  const accountValid = /^\d{10}$/.test(accountNumber);
  const canVerify = bankCode !== '' && accountValid && !verify.isPending;

  const changeBank = (code: string) => {
    setBankCode(code);
    setResolution(null);
    verify.reset();
  };
  const changeAccount = (value: string) => {
    setAccountNumber(value.replace(/\D/g, '').slice(0, 10));
    setResolution(null);
    verify.reset();
  };

  const runVerify = () => {
    if (!canVerify) return;
    verify.mutate(
      { bank_code: bankCode, account_number: accountNumber },
      {
        onSuccess: (data) => {
          setResolution({
            resolutionId: data.resolution_id,
            accountName: data.account_name,
            bankCode,
            bankName: selectedBank?.name ?? bankCode,
            accountNumber,
          });
        },
      },
    );
  };

  const confirm = () => {
    if (!resolution) return;
    onConfirm({
      recipient: {
        rail: 'NG_BANK_TRANSFER',
        bank_code: resolution.bankCode,
        account_number: resolution.accountNumber,
        resolution_id: resolution.resolutionId,
      },
      displayName: resolution.accountName,
      summary: `${resolution.bankName} · ${maskTail(resolution.accountNumber)}`,
    });
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <label htmlFor="bank-filter" className="block text-sm font-medium">
          Bank
        </label>
        <input
          id="bank-filter"
          type="search"
          placeholder="Search bank name"
          value={bankFilter}
          onChange={(e) => setBankFilter(e.target.value)}
          className="form-input"
          disabled={banks.isPending}
        />
        <select
          aria-label="Bank list"
          size={Math.min(6, Math.max(visibleBanks.length, 2))}
          value={bankCode}
          onChange={(e) => changeBank(e.target.value)}
          className="form-input"
        >
          {visibleBanks.map((b) => (
            <option key={b.code} value={b.code}>
              {b.name}
            </option>
          ))}
        </select>
        {banks.isError && (
          <p role="alert" className="text-sm text-error-700 dark:text-error-300">
            {banks.error.message}
          </p>
        )}
      </div>

      <div className="space-y-2">
        <label htmlFor="account-number" className="block text-sm font-medium">
          Account number (10 digits)
        </label>
        <input
          id="account-number"
          type="text"
          inputMode="numeric"
          autoComplete="off"
          placeholder="0123456789"
          value={accountNumber}
          onChange={(e) => changeAccount(e.target.value)}
          className="form-input font-mono tracking-wider"
          aria-describedby="account-hint"
        />
        <p id="account-hint" className="text-xs text-muted">
          {accountNumber.length > 0 && !accountValid
            ? `${10 - accountNumber.length} more digits needed`
            : 'We check the account name with the bank before you pay.'}
        </p>
      </div>

      {!resolution && (
        <button type="button" onClick={runVerify} disabled={!canVerify} className="btn btn-secondary w-full">
          {verify.isPending ? 'Checking account…' : 'Check account'}
        </button>
      )}

      {verify.isError && (
        <p role="alert" className="rounded-lg bg-error-50 px-3 py-2 text-sm text-error-700 dark:bg-error-950 dark:text-error-200">
          {verify.error.message}
        </p>
      )}

      {resolution && (
        <section aria-labelledby="confirm-holder" className="card card-content space-y-3 border-success-600">
          <h3 id="confirm-holder" className="flex items-center gap-2 font-semibold">
            <CheckCircle2 className="h-5 w-5 text-success-700 dark:text-success-300" aria-hidden="true" />
            Confirm account holder
          </h3>
          <p className="text-lg font-semibold">{resolution.accountName}</p>
          <p className="text-sm text-muted">
            {resolution.bankName} · {maskTail(resolution.accountNumber)}
          </p>
          <p className="text-sm text-muted">Check the name matches the person you are paying. Changing the bank or account will ask you to check again.</p>
          <button type="button" onClick={confirm} className="btn btn-primary w-full">
            Yes, this is the account holder
          </button>
        </section>
      )}
    </div>
  );
}
