import { PaymentView } from '@/features/payment/PaymentView';

export default async function PaymentPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <main className="container-safe max-w-md">
      <PaymentView id={id} />
    </main>
  );
}
