import React from "react";

export interface RunnerProps {
  title: string;
}

export const Runner = ({ title }: RunnerProps) => (
  <main data-testid="runner">{title}</main>
);
