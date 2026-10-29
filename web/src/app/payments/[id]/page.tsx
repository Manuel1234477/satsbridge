import { PaymentView } from '@/features/payment/PaymentView';

export default async function PaymentPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <div className="page narrow">
      <PaymentView id={id} />
    </div>
  );
}
