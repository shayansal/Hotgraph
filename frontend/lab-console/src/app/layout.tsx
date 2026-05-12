import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Reality Graph Lab Console",
  description: "Executive command console for AI memory and retrieval quality"
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
