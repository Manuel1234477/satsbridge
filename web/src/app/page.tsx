import { SendWizard } from '@/features/send/SendWizard';

export default async function Home({ searchParams }: { searchParams: Promise<{ from?: string }> }) {
  const { from } = await searchParams;
  return <SendWizard fromPaymentId={from} />;
}
