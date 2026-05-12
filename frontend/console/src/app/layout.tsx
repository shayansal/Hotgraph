import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Reality Graph Console",
  description: "Admin console for Reality Graph"
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
