// Test fixture: covers generics, decorators, modules.

export interface Repository<T> {
    save(item: T): Promise<void>;
    findById(id: string): Promise<T | null>;
}

export class InMemoryRepo<T extends { id: string }> implements Repository<T> {
    private store: Map<string, T> = new Map();

    async save(item: T): Promise<void> {
        this.store.set(item.id, item);
    }

    async findById(id: string): Promise<T | null> {
        return this.store.get(id) ?? null;
    }

    countAll(): number {
        return this.store.size;
    }
}

export function logged<T extends (...args: any[]) => any>(fn: T): T {
    return ((...args: Parameters<T>): ReturnType<T> => {
        console.log(`calling ${fn.name}`);
        return fn(...args);
    }) as T;
}

export type User = { id: string; name: string };

export const repo = new InMemoryRepo<User>();
export { Repository as Repo };
