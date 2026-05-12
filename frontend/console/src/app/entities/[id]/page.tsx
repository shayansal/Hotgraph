import { EntityDetail } from "@/components/EntityDetail";

export default async function EntityPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <EntityDetail entityId={decodeURIComponent(id)} />;
}
