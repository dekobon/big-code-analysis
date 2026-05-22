public class Hello {
    public static String greet(String name) {
        if (name == null || name.isEmpty()) {
            return "hello, world";
        }
        return "hello, " + name;
    }
}
