import { SendWizard } from '@/features/send/SendWizard';

export default async function Home({ searchParams }: { searchParams: Promise<{ from?: string }> }) {
  const { from } = await searchParams;
  return (
    <main className="container-safe max-w-md">
      <SendWizard fromPaymentId={from} />
    </main>
  );
}
