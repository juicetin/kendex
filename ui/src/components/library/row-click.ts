/** A click on a mark inside a row: it opens what the mark names, and the
 *  row does not also open its own first install underneath it. */
export const markClick =
  (open: () => void) =>
  (event: { stopPropagation: () => void }): void => {
    event.stopPropagation();
    open();
  };
