export type Mailbox = { name: string | null; addr: string };

export type Folder = { key: string; name: string; total: number; unread: number };

export type Row = {
  id: number;
  thread: number;
  date: number;
  folder: string;
  from: Mailbox | null;
  subject: string;
  snippet: string;
  recipient: string | null;
  unread: boolean;
  flagged: boolean;
  answered: boolean;
  attachments: boolean;
};

export type Conversation = Row & { messages: number };

export type Attachment = { name: string; mime: string; size: number };

export type Message = Row & {
  message_id: string;
  to: Mailbox[];
  cc: Mailbox[];
  reply_to: Mailbox[];
  attachment_list: Attachment[];
  html: string | null;
  text: string | null;
  remote_images: number;
};

export type Draft = {
  from: string;
  to: Mailbox[];
  cc: Mailbox[];
  subject: string;
  reply_to: number;
};

export type AddressGroup = {
  domain: string;
  catch_all: boolean;
  addresses: { address: string; count: number; mailbox: boolean }[];
};

export type Op = "read" | "unread" | "flag" | "unflag" | "archive" | "junk" | "inbox" | "move" | "delete";

export type Outgoing = {
  from: string;
  to: Mailbox[];
  cc: Mailbox[];
  bcc: Mailbox[];
  subject: string;
  text: string;
  reply_to?: number;
  draft?: string;
};

export type SavedDraft = {
  draft: string;
  from: string | null;
  to: Mailbox[];
  cc: Mailbox[];
  bcc: Mailbox[];
  subject: string;
  text: string;
  reply_to: number | null;
};
