import { NextRequest, NextResponse } from "next/server";

const API_BASE = process.env.RG_API_URL ?? process.env.NEXT_PUBLIC_RG_API_URL ?? "http://127.0.0.1:8080";

type RouteContext = {
  params: Promise<{
    path: string[];
  }>;
};

export async function GET(request: NextRequest, context: RouteContext) {
  return forward(request, context);
}

export async function POST(request: NextRequest, context: RouteContext) {
  return forward(request, context);
}

async function forward(request: NextRequest, context: RouteContext) {
  const params = await context.params;
  const targetUrl = new URL(`${API_BASE}/${params.path.join("/")}`);
  targetUrl.search = request.nextUrl.search;
  const body = request.method === "GET" ? undefined : await request.text();
  const response = await fetch(targetUrl, {
    method: request.method,
    headers: {
      "content-type": request.headers.get("content-type") ?? "application/json"
    },
    body,
    cache: "no-store"
  });
  const text = await response.text();
  return new NextResponse(text, {
    status: response.status,
    headers: {
      "content-type": response.headers.get("content-type") ?? "application/json"
    }
  });
}
