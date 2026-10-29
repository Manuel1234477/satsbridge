'use client';

import { useState, useEffect } from 'react';
import { useRecipientResolution } from '@/hooks/useRecipientResolution';
import { getPayoutInstitutions } from '@/lib/api';
import { AlertCircle, CheckCircle } from 'lucide-react';
import type { PayoutInstitution } from '@/lib/api/schemas';

interface NgBankRecipientProps {
  currency: string;
  onResolve: (data: { bank_code: string; account_number: string; resolution_id: string; account_name: string }) => void;
}

export function NgBankRecipient({ currency, onResolve }: NgBankRecipientProps) {
  const [bankCode, setBankCode] = useState('');
  const [accountNumber, setAccountNumber] = useState('');
  const [banks, setBanks] = useState<PayoutInstitution[]>([]);
  const [searchBank, setSearchBank] = useState('');
  const [showBankList, setShowBankList] = useState(false);
  const [isLoadingBanks, setIsLoadingBanks] = useState(true);

  const resolutionMutation = useRecipientResolution();

  // Load banks on mount
  useEffect(() => {
    getPayoutInstitutions(currency, 'NG_BANK_TRANSFER')
      .then(setBanks)
      .catch(console.error)
      .finally(() => setIsLoadingBanks(false));
  }, [currency]);

  const filteredBanks = banks.filter(
    b => b.code.includes(searchBank.toUpperCase()) || b.name.toUpperCase().includes(searchBank.toUpperCase())
  );

  const selectedBank = banks.find(b => b.code === bankCode);

  const handleResolve = () => {
    if (!bankCode || !accountNumber) return;

    resolutionMutation.mutate(
      {
        currency,
        bank_code: bankCode,
        account_number: accountNumber,
      },
      {
        onSuccess: (resolution: any) => {
          onResolve({
            bank_code: bankCode,
            account_number: accountNumber,
            resolution_id: resolution.resolution_id,
            account_name: resolution.account_name,
          });
        },
      }
    );
  };

  const isValid = bankCode && accountNumber.length === 10;
  const isResolved = resolutionMutation.isSuccess;

  return (
    <div className="space-y-4">
      {/* Bank selector */}
      <div className="space-y-2">
        <label className="block text-sm font-medium">Bank</label>
        <div className="relative">
          <input
            type="text"
            placeholder="Search bank name or code..."
            value={showBankList ? searchBank : selectedBank?.name || ''}
            onChange={(e) => {
              setSearchBank(e.target.value);
              setShowBankList(true);
            }}
            onFocus={() => setShowBankList(true)}
            className="form-input"
            disabled={isLoadingBanks}
          />
          {showBankList && filteredBanks.length > 0 && (
            <div className="absolute top-full left-0 right-0 mt-1 border border-neutral-200 dark:border-neutral-700 rounded-lg bg-white dark:bg-neutral-900 shadow-lg max-h-48 overflow-y-auto z-10">
              {filteredBanks.map((bank) => (
                <button
                  key={bank.code}
                  type="button"
                  onClick={() => {
                    setBankCode(bank.code);
                    setShowBankList(false);
                    setSearchBank('');
                  }}
                  className="w-full text-left px-4 py-2 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                >
                  <div className="font-medium">{bank.name}</div>
                  <div className="text-xs text-muted">{bank.code}</div>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Account number */}
      <div className="space-y-2">
        <label className="block text-sm font-medium">Account number (10 digits)</label>
        <input
          type="text"
          placeholder="0123456789"
          value={accountNumber}
          onChange={(e) => {
            const digits = e.target.value.replace(/\D/g, '').slice(0, 10);
            setAccountNumber(digits);
          }}
          maxLength={10}
          className="form-input font-mono"
        />
        {accountNumber.length > 0 && accountNumber.length < 10 && (
          <p className="text-xs text-muted">{10 - accountNumber.length} digits remaining</p>
        )}
      </div>

      {/* Resolution result */}
      {isResolved && resolutionMutation.data && (
        <div className="flex gap-3 p-3 rounded-lg bg-success-50 dark:bg-success-950 border border-success-200 dark:border-success-800">
          <CheckCircle className="w-5 h-5 text-success-600 dark:text-success-400 flex-shrink-0 mt-0.5" />
          <div>
            <p className="text-sm font-medium text-success-900 dark:text-success-100">Account verified</p>
            <p className="text-sm text-success-700 dark:text-success-300">{resolutionMutation.data.account_name}</p>
          </div>
        </div>
      )}

      {/* Error */}
      {resolutionMutation.isError && (
        <div className="flex gap-2 p-3 rounded-lg bg-error-50 dark:bg-error-950 text-error-700 dark:text-error-300">
          <AlertCircle className="w-5 h-5 flex-shrink-0 mt-0.5" />
          <p className="text-sm">{resolutionMutation.error?.message}</p>
        </div>
      )}

      {/* Resolve button */}
      <button
        type="button"
        onClick={handleResolve}
        disabled={!isValid || resolutionMutation.isPending}
        className="btn btn-primary w-full"
      >
        {resolutionMutation.isPending ? 'Verifying...' : 'Verify account'}
      </button>
    </div>
  );
}
