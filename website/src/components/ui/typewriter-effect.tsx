import { motion, useReducedMotion } from "motion/react";
import { cn } from "@/lib/utils";

export type TypewriterWord = {
  text: string;
  className?: string;
};

type TypewriterProps = {
  words: TypewriterWord[];
  className?: string;
  cursorClassName?: string;
};

export function TypewriterEffectSmooth({
  words,
  className,
  cursorClassName,
}: TypewriterProps) {
  const reduceMotion = useReducedMotion();
  const sentence = words.map((word) => word.text).join(" ");

  const content = (
    <>
      {words.map((word, index) => (
        <span key={`word-${index}`} className="inline-block whitespace-pre">
          {word.text.split("").map((char, charIndex) => (
            <span
              key={`char-${charIndex}`}
              className={cn("inline-block", word.className)}
            >
              {char}
            </span>
          ))}
          {index < words.length - 1 && " "}
        </span>
      ))}
      <motion.span
        aria-hidden
        className={cn(
          "ml-[0.06em] inline-block h-[0.82em] w-[0.045em] translate-y-[0.1em] rounded-[1px] bg-brand",
          cursorClassName,
        )}
        initial={{ opacity: reduceMotion ? 1 : 0 }}
        animate={reduceMotion ? undefined : { opacity: 1 }}
        transition={
          reduceMotion
            ? undefined
            : { duration: 0.8, repeat: Infinity, repeatType: "reverse" }
        }
      />
    </>
  );

  // Reduced motion: the title is simply there, no reveal, no blink.
  if (reduceMotion) {
    return <span className={cn("inline-block", className)}>{content}</span>;
  }

  // The invisible copy reserves the final width (so the centered layout
  // never shifts); the clipped copy above it grows across that width,
  // revealing the characters left to right with the cursor riding the edge.
  // The outer span is the in-view observer — a zero-width target would
  // never intersect.
  return (
    <motion.span
      className="relative inline-block align-bottom"
      initial="hidden"
      whileInView="visible"
      viewport={{ once: true }}
      aria-label={sentence}
    >
      <span aria-hidden className={cn("invisible inline-block", className)}>
        {sentence}
      </span>
      <motion.span
        aria-hidden
        className={cn(
          "absolute inset-y-0 left-0 inline-block overflow-hidden",
          className,
        )}
        variants={{
          hidden: { width: "0%" },
          visible: {
            width: "100%",
            transition: { duration: 2, ease: "linear", delay: 1 },
          },
        }}
      >
        <span className="inline-block whitespace-nowrap">{content}</span>
      </motion.span>
    </motion.span>
  );
}

export function TypewriterEffect({ words, className, cursorClassName }: TypewriterProps) {
  const reduceMotion = useReducedMotion();

  return (
    <span
      className={cn("inline-block", className)}
      aria-label={words.map((word) => word.text).join(" ")}
    >
      {words.map((word, index) => (
        <span key={`word-${index}`} className="inline-block whitespace-pre">
          {word.text.split("").map((char, charIndex) => (
            <motion.span
              key={`char-${charIndex}`}
              className={cn("inline-block", word.className)}
              initial={reduceMotion ? { opacity: 1 } : { opacity: 0, display: "none" }}
              whileInView={
                reduceMotion ? undefined : { opacity: 1, display: "inline-block" }
              }
              viewport={{ once: true }}
              transition={
                reduceMotion
                  ? undefined
                  : {
                      duration: 0.3,
                      delay: 0.1 * (charIndex + index * 6),
                      ease: "easeInOut",
                    }
              }
            >
              {char}
            </motion.span>
          ))}
          {index < words.length - 1 && " "}
        </span>
      ))}
      <motion.span
        aria-hidden
        className={cn(
          "ml-[0.06em] inline-block h-[0.82em] w-[0.045em] translate-y-[0.1em] rounded-[1px] bg-brand",
          cursorClassName,
        )}
        initial={{ opacity: reduceMotion ? 1 : 0 }}
        animate={reduceMotion ? undefined : { opacity: 1 }}
        transition={
          reduceMotion
            ? undefined
            : { duration: 0.8, repeat: Infinity, repeatType: "reverse" }
        }
      />
    </span>
  );
}
