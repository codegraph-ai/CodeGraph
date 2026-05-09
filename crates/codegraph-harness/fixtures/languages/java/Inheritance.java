package fixtures.basic;

import java.util.List;
import java.util.ArrayList;

// Test fixture: covers inheritance, generics, packages.

interface Walker {
    void walk();
}

abstract class Animal {
    protected final String name;

    public Animal(String name) {
        this.name = name;
    }

    public abstract String describe();

    public String getName() {
        return name;
    }
}

public class Inheritance {

    public static class Dog extends Animal implements Walker {
        private final String breed;

        public Dog(String name, String breed) {
            super(name);
            this.breed = breed;
        }

        @Override
        public String describe() {
            return name + " (" + breed + ")";
        }

        @Override
        public void walk() {
            System.out.println(name + " is walking");
        }
    }

    public static class Box<T extends Comparable<T>> {
        private final List<T> items = new ArrayList<>();

        public void add(T item) {
            items.add(item);
        }

        public T max() {
            T best = items.get(0);
            for (T item : items) {
                if (item.compareTo(best) > 0) {
                    best = item;
                }
            }
            return best;
        }
    }
}
