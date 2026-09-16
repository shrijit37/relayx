import { Toaster as Sonner } from "sonner";

type ToasterProps = React.ComponentProps<typeof Sonner>;

const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      className="toaster group"
      theme="dark"
      closeButton
      duration={4000}
      toastOptions={{
        classNames: {
          toast: "toast",
          title: "text-[13px] font-medium",
          description: "text-xs text-muted-foreground",
          closeButton:
            "bg-panel-raised border-border text-muted-foreground hover:bg-panel-raised hover:border-border-strong",
          actionButton:
            "bg-primary text-primary-foreground hover:bg-primary/90 rounded-sm text-xs",
          cancelButton:
            "bg-muted text-muted-foreground hover:bg-panel-raised rounded-sm text-xs",
        },
      }}
      {...props}
    />
  );
};

export { Toaster };
