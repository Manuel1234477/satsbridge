'use client';

import { useMemo, useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { getPayoutInstitutions, resolveRecipient } from '@/lib/api';
import type { PayoutInstitution } from '@/lib/api/schemas';
import { Sheet } from '@/components/ui/Sheet';
import { CurrencyBadge } from '@/components/ui/CurrencyChip';
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

function groupDigits(digits: string): string {
  return digits.replace(/(\d{3})(?=\d)/g, '$1 ');
}

function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? '')
    .join('');
}

export function NgBankRecipient({ onConfirm }: NgBankRecipientProps) {
  const [bankCode, setBankCode] = useState('');
  const [bankSheet, setBankSheet] = useState(false);
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
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-2">
        <span className="label">Bank</span>
        <button type="button" className="field flex items-center justify-between text-left" aria-haspopup="dialog" onClick={() => setBankSheet(true)} disabled={banks.isPending}>
          <span>{selectedBank ? selectedBank.name : banks.isPending ? 'Loading banks…' : 'Choose bank'}</span>
          <span aria-hidden="true" className="text-[var(--text-2)]">▾</span>
        </button>
        {banks.isError && (
          <p role="alert" className="error-text">
            {banks.error.message}
          </p>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <label htmlFor="account-number" className="label">
          Account number (10 digits)
        </label>
        <input
          id="account-number"
          type="text"
          inputMode="numeric"
          autoComplete="off"
          placeholder="012 345 6789"
          value={groupDigits(accountNumber)}
          onChange={(e) => changeAccount(e.target.value)}
          className="field money text-[22px] tracking-wider"
          aria-describedby="account-hint"
        />
        <p id="account-hint" className="label">
          {accountNumber.length > 0 && !accountValid
            ? `${10 - accountNumber.length} more digits needed`
            : 'We check the account name with the bank before you pay.'}
        </p>
      </div>

      {!resolution && (
        <button type="button" onClick={runVerify} disabled={!canVerify} className="btn btn-quiet btn-block">
          {verify.isPending ? 'Checking account…' : 'Check account'}
        </button>
      )}

      {verify.isError && (
        <p role="alert" className="notice notice-failed">
          {verify.error.message}
        </p>
      )}

      {resolution && (
        <section aria-labelledby="confirm-holder" className="card-raised flex flex-col items-center gap-3 text-center">
          <h3 id="confirm-holder" className="label">
            Confirm account holder
          </h3>
          <span className="badge" style={{ width: 56, height: 56, fontSize: 18 }} aria-hidden="true">
            {initials(resolution.accountName)}
          </span>
          <p className="text-[22px] font-semibold">{resolution.accountName}</p>
          <p className="label">
            {resolution.bankName} · {maskTail(resolution.accountNumber)}
          </p>
          <p className="label">Check the name matches the person you are paying. Changing the bank or account asks you to check again.</p>
          <button type="button" onClick={confirm} className="btn btn-accent btn-block">
            Yes, this is correct
          </button>
        </section>
      )}

      <Sheet open={bankSheet} onClose={() => setBankSheet(false)} title="Choose bank">
        <input
          type="search"
          className="field"
          placeholder="Search bank name or code"
          aria-label="Search banks"
          value={bankFilter}
          onChange={(e) => setBankFilter(e.target.value)}
        />
        <div className="sheet-list" role="listbox" aria-label="Banks">
          {visibleBanks.map((b) => (
            <button
              key={b.code}
              type="button"
              role="option"
              aria-selected={b.code === bankCode}
              className="sheet-option"
              onClick={() => {
                changeBank(b.code);
                setBankSheet(false);
              }}
            >
              <CurrencyBadge code={b.name.replace(/[^A-Za-z]/g, '')} />
              <span>{b.name}</span>
            </button>
          ))}
          {visibleBanks.length === 0 && <p className="p-3 text-[var(--text-2)]">No bank matches “{bankFilter}”.</p>}
        </div>
      </Sheet>
    </div>
  );
}
