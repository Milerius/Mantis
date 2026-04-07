#pragma once

#include <cstddef>

#include "rigtorp/SPSCQueue.h"
#include "../message.hpp"

/// Adapter for rigtorp::SPSCQueue.
struct RigtorpAdapter {
    static constexpr const char* name = "rigtorp";

    rigtorp::SPSCQueue<Message48> queue;

    explicit RigtorpAdapter(size_t capacity) : queue(capacity) {}

    auto make_push() {
        return [this](const Message48& msg) -> bool {
            return queue.try_push(msg);
        };
    }

    auto make_pop() {
        return [this](Message48& out) -> bool {
            auto* front = queue.front();
            if (!front) return false;
            out = *front;
            queue.pop();
            return true;
        };
    }
};
