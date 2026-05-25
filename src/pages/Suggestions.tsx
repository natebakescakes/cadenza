import React, { useState } from "react";
import { motion } from "framer-motion";
import {
  BookOpen,
  EyeOff,
  Lightbulb,
  Plus,
  Search,
  TimerReset,
} from "lucide-react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ProgressBar } from "@/components/ProgressBar";
import { SortableHead } from "@/components/SortableHead";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useSuggestions } from "@/hooks/useSuggestions";
import { useWords } from "@/hooks/useWords";
import { useHiddenWords } from "@/hooks/useHiddenWords";
import { useSort } from "@/hooks/useSort";
import { formatMs, formatNumber, formatPercent, formatRelative } from "@/lib/format";
import type { Suggestion, WordRecord } from "@/lib/types";

type Tab = "words" | "suggestions";

export default function Suggestions() {
  const [tab, setTab] = useState<Tab>("words");
  const [search, setSearch] = useState("");

  return (
    <div>
      <PageHeader
        title="Words"
        subtitle="Your complete typing history — every word, its accuracy, and chord opportunities."
        actions={
          <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
            <TabsList>
              <TabsTrigger value="words">
                <BookOpen className="size-3.5 mr-1.5" /> All words
              </TabsTrigger>
              <TabsTrigger value="suggestions">
                <Lightbulb className="size-3.5 mr-1.5" /> Chord candidates
              </TabsTrigger>
            </TabsList>
          </Tabs>
        }
      />

      {tab === "words" ? (
        <WordsTable search={search} setSearch={setSearch} />
      ) : (
        <SuggestionsTable />
      )}
    </div>
  );
}

function WordsTable({
  search,
  setSearch,
}: {
  search: string;
  setSearch: (s: string) => void;
}) {
  const { data, refresh } = useWords(500, "score", search);
  const { hide } = useHiddenWords();
  const { sorted, sortKey, sortDir, toggle } = useSort<WordRecord>(data, "score");

  const handleHide = async (word: string) => {
    await hide(word);
    toast.success(`"${word}" hidden from Words and suggestions.`);
    void refresh();
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="space-y-3"
    >
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          placeholder="Filter words…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-9"
        />
      </div>

      <Card>
        <CardContent className="px-0">
          {sorted.length ? (
            <TooltipProvider>
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <SortableHead columnKey="word" sortKey={sortKey} sortDir={sortDir} onSort={toggle} className="pl-5">
                    Word
                  </SortableHead>
                  <SortableHead columnKey="frequency" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                    Times typed
                  </SortableHead>
                  <SortableHead columnKey="accuracy_rate" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                    Accuracy
                  </SortableHead>
                  <SortableHead columnKey="avg_speed_ms" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                    Avg speed
                  </SortableHead>
                  <SortableHead columnKey="score" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                    Score
                  </SortableHead>
                  <SortableHead columnKey="last_used" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                    Last used
                  </SortableHead>
                  <TableHead className="pr-5 w-10" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {sorted.map((w) => {
                  const acc = w.accuracy_rate;
                  const accTone = acc >= 0.9 ? "success" : acc >= 0.6 ? "warning" : "danger";
                  return (
                    <TableRow key={w.word} className="group">
                      <TableCell className="pl-5 font-mono text-sm font-medium text-foreground">
                        {w.word}
                      </TableCell>
                      <TableCell className="tnum text-right text-muted-foreground">
                        {formatNumber(w.frequency)}×
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex items-center justify-end gap-2">
                          <div className="w-16">
                            <ProgressBar value={acc} tone={accTone} size="sm" aria-label={`${w.word} accuracy`} />
                          </div>
                          <span className="tnum w-10 text-xs text-muted-foreground">
                            {formatPercent(acc)}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell className="tnum text-right text-muted-foreground">
                        {formatMs(w.avg_speed_ms)}
                      </TableCell>
                      <TableCell className="text-right">
                        <Badge variant="outline" className="tnum text-gold">
                          {Math.round(w.score)}
                        </Badge>
                      </TableCell>
                      <TableCell className="tnum text-right text-muted-foreground">
                        {formatRelative(w.last_used)}
                      </TableCell>
                      <TableCell className="pr-5 text-right">
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="icon"
                              variant="ghost"
                              className="size-7 opacity-0 transition-opacity text-muted-foreground group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100"
                              aria-label={`Hide ${w.word}`}
                              onClick={() => void handleHide(w.word)}
                            >
                              <EyeOff className="size-3.5" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="left">Hide word</TooltipContent>
                        </Tooltip>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
            </TooltipProvider>
          ) : (
            <EmptyState
              icon={BookOpen}
              title="No words yet"
              hint="Start typing — every word you produce will appear here with accuracy and speed stats."
            />
          )}
        </CardContent>
      </Card>
    </motion.div>
  );
}

function SuggestionsTable() {
  const { data, refresh } = useSuggestions(100);
  const { hide } = useHiddenWords();
  const { sorted, sortKey, sortDir, toggle } = useSort<Suggestion>(data, "score");
  const totalSaving = data.reduce((sum, s) => sum + s.projected_saving_ms, 0);

  const handleHide = async (phrase: string) => {
    await hide(phrase);
    toast.success(`"${phrase}" hidden from suggestions.`);
    void refresh();
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
    >
      {data.length > 0 && (
        <div className="mb-3 flex justify-end">
          <Badge variant="outline" className="gap-1.5 py-1.5 text-xs">
            <TimerReset className="size-3.5 text-gold" />
            <span className="text-muted-foreground">Potential save</span>
            <span className="tnum font-semibold text-foreground">
              {formatMs(totalSaving)}
            </span>
          </Badge>
        </div>
      )}
      <Card>
        <CardContent className="px-0">
          {sorted.length ? (
            <TooltipProvider>
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <SortableHead columnKey="phrase" sortKey={sortKey} sortDir={sortDir} onSort={toggle} className="pl-5">
                      Phrase
                    </SortableHead>
                    <SortableHead columnKey="frequency" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                      Frequency
                    </SortableHead>
                    <SortableHead columnKey="score" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                      Score
                    </SortableHead>
                    <TableHead className="text-right text-xs font-medium tracking-wider text-muted-foreground/70 uppercase">
                      Combo
                    </TableHead>
                    <SortableHead columnKey="avg_manual_ms" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                      Avg manual
                    </SortableHead>
                    <SortableHead columnKey="projected_saving_ms" sortKey={sortKey} sortDir={sortDir} onSort={toggle} align="right">
                      Time saved
                    </SortableHead>
                    <TableHead className="text-right text-xs font-medium tracking-wider text-muted-foreground/70 uppercase">
                      Action
                    </TableHead>
                    <TableHead className="pr-5 w-10" />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {sorted.map((s) => (
                    <TableRow key={s.phrase} className="group">
                      <TableCell className="pl-5 font-mono text-sm text-foreground">
                        {s.phrase}
                      </TableCell>
                      <TableCell className="tnum text-right text-muted-foreground">
                        {formatNumber(s.frequency)}×
                      </TableCell>
                      <TableCell className="text-right">
                        <Badge variant="outline" className="tnum text-gold">
                          {Math.round(s.score)}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex flex-col items-end gap-1.5">
                          {s.combos.length === 0 ? (
                            <span className="text-muted-foreground text-xs">—</span>
                          ) : (
                            s.combos.map((combo, i) => (
                              <div key={i} className={`flex items-center gap-1 flex-wrap justify-end${i > 0 ? " opacity-50" : ""}`}>
                                {combo.kind === "compound" ? (
                                  combo.parts.map((part, j) => (
                                    <React.Fragment key={j}>
                                      {j > 0 && (
                                        <span className="text-muted-foreground text-xs mx-0.5">→</span>
                                      )}
                                      <span className="flex items-center gap-0.5">
                                        {part.split(" + ").map((key) => (
                                          <Badge
                                            key={key}
                                            variant="outline"
                                            className="font-mono text-xs px-1 py-0 h-4"
                                          >
                                            {key}
                                          </Badge>
                                        ))}
                                      </span>
                                    </React.Fragment>
                                  ))
                                ) : (
                                  combo.parts[0]?.split(" + ").map((key) => (
                                    <Badge
                                      key={key}
                                      variant="outline"
                                      className="font-mono text-xs px-1.5 py-0.5"
                                    >
                                      {key}
                                    </Badge>
                                  ))
                                )}
                                {combo.conflicts.length > 0 && (
                                  <Tooltip>
                                    <TooltipTrigger asChild>
                                      <span className="text-warning cursor-help text-xs ml-0.5">⚠</span>
                                    </TooltipTrigger>
                                    <TooltipContent side="left" className="max-w-48">
                                      Conflicts: {combo.conflicts.join(", ")}
                                    </TooltipContent>
                                  </Tooltip>
                                )}
                              </div>
                            ))
                          )}
                        </div>
                      </TableCell>
                      <TableCell className="tnum text-right text-muted-foreground">
                        {formatMs(s.avg_manual_ms)}
                      </TableCell>
                      <TableCell className="tnum text-right font-medium text-success">
                        {formatMs(s.projected_saving_ms)}
                      </TableCell>
                      <TableCell className="text-right">
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span className="inline-flex">
                              <Button size="sm" variant="secondary" disabled className="pointer-events-none">
                                <Plus className="size-3.5" /> Create chord
                              </Button>
                            </span>
                          </TooltipTrigger>
                          <TooltipContent side="left">
                            Push to device — coming soon
                          </TooltipContent>
                        </Tooltip>
                      </TableCell>
                      <TableCell className="pr-5 text-right">
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="icon"
                              variant="ghost"
                              className="size-7 opacity-0 transition-opacity text-muted-foreground group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100"
                              aria-label={`Hide ${s.phrase}`}
                              onClick={() => void handleHide(s.phrase)}
                            >
                              <EyeOff className="size-3.5" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="left">Hide suggestion</TooltipContent>
                        </Tooltip>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TooltipProvider>
          ) : (
            <EmptyState
              icon={Lightbulb}
              title="No suggestions yet"
              hint="As you type, Cadenza watches for words you spell out manually again and again. The best chord candidates will appear here, ranked by how much time a chord would save."
            />
          )}
        </CardContent>
      </Card>
    </motion.div>
  );
}
