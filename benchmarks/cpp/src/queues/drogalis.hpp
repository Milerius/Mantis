#pragma once

#include <cstddef>

#include "dro/spsc-queue.hpp"
#include "../message.hpp"

/// Adapter for dro::SPSCQueue.
struct DrogalisAdapter {
    static constexpr const char* name = "drogalis";

    dro::SPSCQueue<Message48> queue;

    explicit DrogalisAdapter(size_t capacity) : queue(capacity) {}

    auto make_push() {
        return [this](const Message48& msg) -> bool {
            return queue.try_push(msg);
        };
    }

    auto make_pop() {
        return [this](Message48& out) -> bool {
            return queue.try_pop(out);
        };
    }
};
